import { effect } from "@preact/signals"
import { castDraft } from "immer"
import { createShortcuts, type BindingSet, type BindingSpec, type ShortcutRuntime } from "powerkeys"
import { SigmaType, type SigmaRef } from "preact-sigma"

import { appCommandList, resolveAppCommand } from "~/commands/app-command.ts"
import { commandContext } from "~/commands/command-context.ts"
import { desktopHost } from "~/desktop-host.ts"
import type { AppCommandId } from "~/shared/app-commands.ts"
import {
  createShortcutBinding,
  createDefaultShortcutKeymapFile,
  getShortcutBindingExpression,
  resolveShortcutBindings,
  shortcutKeymapProfiles,
  type ShortcutBinding,
  type KeymapProfileId,
  type ShortcutKeymapBindings,
  type ShortcutKeymapOverrides,
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

function getWriteErrorMessage(error: unknown) {
  if (error instanceof Error && error.message) {
    return error.message
  }

  return typeof error === "string" && error.length > 0 ? error : "Unknown error."
}

function areBindingsEqual(
  left: readonly ShortcutBinding[] | undefined,
  right: readonly ShortcutBinding[] | undefined,
) {
  return JSON.stringify(left ?? []) === JSON.stringify(right ?? [])
}

function normalizeWhenClause(whenClause: string | null | undefined) {
  const trimmedWhenClause = whenClause?.trim()
  return trimmedWhenClause ? trimmedWhenClause : undefined
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
      this.writeError = null
      this.isHydrated = true
      this.rebindRuntime()
      this.commit()
    },

    /** Persists one override snapshot and reapplies the runtime bindings when it succeeds. */
    async persistOverrides(overrides: ShortcutKeymapOverrides) {
      try {
        const keymap = await desktopHost.writeShortcutKeymap({
          version: 1,
          profile: this.selectedProfileId,
          overrides,
        })

        this.applyKeymapSnapshot(keymap.profile, keymap.overrides, this.loadError)
        return true
      } catch (error) {
        this.writeError = `Failed to save shortcut overrides: ${getWriteErrorMessage(error)}`
        this.commit()
        return false
      }
    },

    /** Recomputes one command override from its next effective binding list and persists it. */
    async persistCommandBindings(commandId: AppCommandId, bindings: readonly ShortcutBinding[]) {
      const nextOverrides: ShortcutKeymapOverrides = {
        ...this.overrides,
      }
      const profileBindings: ShortcutKeymapBindings =
        shortcutKeymapProfiles[this.selectedProfileId].bindings
      const defaultBindings = profileBindings[commandId]

      if (bindings.length === 0) {
        if (defaultBindings) {
          nextOverrides[commandId] = null
        } else {
          delete nextOverrides[commandId]
        }
      } else if (areBindingsEqual(defaultBindings, bindings)) {
        delete nextOverrides[commandId]
      } else {
        nextOverrides[commandId] = [...bindings]
      }

      return await this.persistOverrides(nextOverrides)
    },

    /** Appends one recorded shortcut to the command's effective binding list. */
    async addCommandBinding(
      commandId: AppCommandId,
      expression: string,
      whenClause?: string | null,
    ) {
      const normalizedWhenClause = normalizeWhenClause(whenClause)

      return await this.persistCommandBindings(commandId, [
        ...Array.from(
          this.resolvedBindings[commandId] ?? [],
          (binding) => binding as ShortcutBinding,
        ),
        this.toPersistedBinding(commandId, expression, { when: normalizedWhenClause }),
      ])
    },

    /** Replaces one existing binding expression while preserving its binding-local metadata. */
    async replaceCommandBinding(commandId: AppCommandId, bindingIndex: number, expression: string) {
      const nextBindings = Array.from(this.resolvedBindings[commandId] ?? [], (binding) => {
        return binding as ShortcutBinding
      })
      const currentBinding = nextBindings[bindingIndex]

      if (!currentBinding) {
        return false
      }

      nextBindings[bindingIndex] =
        typeof currentBinding === "string"
          ? this.toPersistedBinding(commandId, expression)
          : this.toPersistedBinding(commandId, expression, currentBinding)

      return await this.persistCommandBindings(commandId, nextBindings)
    },

    /** Removes one binding entry from the command. */
    async removeCommandBinding(commandId: AppCommandId, bindingIndex: number) {
      const nextBindings = Array.from(this.resolvedBindings[commandId] ?? [], (binding) => {
        return binding as ShortcutBinding
      })

      if (bindingIndex < 0 || bindingIndex >= nextBindings.length) {
        return false
      }

      nextBindings.splice(bindingIndex, 1)
      return await this.persistCommandBindings(commandId, nextBindings)
    },

    /** Updates one binding-local `when` clause and collapses back to shorthand when possible. */
    async updateCommandBindingWhen(
      commandId: AppCommandId,
      bindingIndex: number,
      whenClause: string | null,
    ) {
      const nextBindings = Array.from(this.resolvedBindings[commandId] ?? [], (binding) => {
        return binding as ShortcutBinding
      })
      const currentBinding = nextBindings[bindingIndex]

      if (!currentBinding) {
        return false
      }

      const expression = getShortcutBindingExpression(currentBinding)

      if (!expression) {
        return false
      }

      nextBindings[bindingIndex] =
        typeof currentBinding === "string"
          ? this.toPersistedBinding(commandId, expression, {
              when: normalizeWhenClause(whenClause),
            })
          : this.toPersistedBinding(commandId, expression, {
              ...currentBinding,
              when: normalizeWhenClause(whenClause),
            })

      return await this.persistCommandBindings(commandId, nextBindings)
    },

    /** Normalizes one expression into the smallest persisted binding entry for the command. */
    toPersistedBinding(
      commandId: AppCommandId,
      expression: string,
      binding?: Omit<Extract<ShortcutBinding, object>, "combo" | "sequence"> | ShortcutBinding,
    ) {
      const commandWhen = resolveAppCommand(commandId)?.when
      const bindingExpression =
        typeof binding === "string"
          ? binding
          : ((binding && "combo" in binding ? binding.combo : undefined) ??
            (binding && "sequence" in binding ? binding.sequence : undefined))
      const bindingWhen =
        binding && typeof binding !== "string" && "when" in binding
          ? normalizeWhenClause(binding.when)
          : undefined
      const nextWhen = bindingWhen === commandWhen ? undefined : bindingWhen
      const options = (
        binding && typeof binding !== "string"
          ? Object.fromEntries(
              Object.entries(binding).filter(([key, value]) => {
                if (key === "combo" || key === "sequence" || key === "when") {
                  return false
                }

                return value !== undefined
              }),
            )
          : {}
      ) as Omit<Extract<ShortcutBinding, object>, "combo" | "sequence">

      if (nextWhen) {
        options.when = nextWhen
      }

      return createShortcutBinding(bindingExpression ?? expression, options)
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
