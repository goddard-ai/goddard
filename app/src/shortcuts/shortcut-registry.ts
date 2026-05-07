import { effect } from "@preact/signals"
import { createShortcuts, type BindingSet, type BindingSpec, type ShortcutRuntime } from "powerkeys"
import { Sigma } from "preact-sigma"

import { appCommandList, resolveAppCommand } from "~/commands/app-command.ts"
import { commandContext } from "~/commands/command-context.ts"
import type { AppCommandId } from "~/shared/app-commands.ts"
import {
  createShortcutBinding,
  getShortcutBindingExpression,
  resolveShortcutBindings,
  shortcutKeymapProfiles,
  type KeymapProfileId,
  type ShortcutBinding,
  type ShortcutKeymapBindings,
  type ShortcutKeymapOverrides,
} from "~/shared/shortcut-keymap.ts"

/** Public state for the app-wide keyboard shortcut registry. */
export type ShortcutRegistryState = {
  selectedProfileId: KeymapProfileId
  overrides: ShortcutKeymapOverrides
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
export class ShortcutRegistry extends Sigma<ShortcutRegistryState> {
  /** Imperative powerkeys runtime that owns document listeners outside persisted shortcut state. */
  #runtime: ShortcutRuntime
  /** Mutable binding set installed in the runtime as the projection of the current keymap state. */
  #bindingSet: BindingSet

  constructor(target: Document | HTMLElement = document) {
    super({
      selectedProfileId: "goddard",
      overrides: {},
    })

    this.#runtime = createShortcuts({
      target,
      editablePolicy: "ignore-editable",
      getActiveScopes: () => commandContext.activeScopes.peek(),
      onError: (error, info) => {
        console.error("Shortcut runtime error.", error, info)
      },
    })
    this.#bindingSet = this.#runtime.createBindingSet()
  }

  get runtime() {
    return this.#runtime
  }

  /** Resolves the active profile and user overrides into effective runtime bindings. */
  get resolvedBindings(): ShortcutKeymapBindings {
    return resolveShortcutBindings(this.selectedProfileId, this.overrides)
  }

  /** Replaces the active profile and override snapshot, then reapplies runtime bindings. */
  applyKeymapSnapshot(profile: KeymapProfileId, overrides: ShortcutKeymapOverrides) {
    this.selectedProfileId = profile
    this.overrides = overrides
    this.commit()
    this.rebindRuntime()
  }

  /** Recomputes one command override from its next effective binding list. */
  setCommandBindings(commandId: AppCommandId, bindings: readonly ShortcutBinding[]) {
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

    this.applyKeymapSnapshot(this.selectedProfileId, nextOverrides)
    return true
  }

  /** Appends one recorded shortcut to the command's effective binding list. */
  addCommandBinding(commandId: AppCommandId, expression: string, whenClause?: string | null) {
    const normalizedWhenClause = normalizeWhenClause(whenClause)

    return this.setCommandBindings(commandId, [
      ...Array.from(
        this.resolvedBindings[commandId] ?? [],
        (binding) => binding as ShortcutBinding,
      ),
      this.toPersistedBinding(commandId, expression, {
        when: normalizedWhenClause,
      }),
    ])
  }

  /** Replaces one existing binding expression while preserving its binding-local metadata. */
  replaceCommandBinding(commandId: AppCommandId, bindingIndex: number, expression: string) {
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

    return this.setCommandBindings(commandId, nextBindings)
  }

  /** Removes one binding entry from the command. */
  removeCommandBinding(commandId: AppCommandId, bindingIndex: number) {
    const nextBindings = Array.from(this.resolvedBindings[commandId] ?? [], (binding) => {
      return binding as ShortcutBinding
    })

    if (bindingIndex < 0 || bindingIndex >= nextBindings.length) {
      return false
    }

    nextBindings.splice(bindingIndex, 1)
    return this.setCommandBindings(commandId, nextBindings)
  }

  /** Updates one binding-local `when` clause and collapses back to shorthand when possible. */
  updateCommandBindingWhen(
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

    return this.setCommandBindings(commandId, nextBindings)
  }

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
  }

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

    this.#bindingSet.replace(nextBindings)
  }

  onSetup() {
    const disposeContextSync = effect(() => {
      this.#runtime.batchContext(commandContext.whenContext.value)
    })

    this.rebindRuntime()

    return [
      () => {
        disposeContextSync()
        this.#bindingSet.dispose()
      },
      this.#runtime,
    ]
  }
}

export interface ShortcutRegistry extends ShortcutRegistryState {}

/** Module-scoped shortcut registry singleton shared across the app shell. */
export const shortcutRegistry = new ShortcutRegistry()
