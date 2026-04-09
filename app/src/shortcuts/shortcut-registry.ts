import { createShortcuts, type ShortcutMatch, type ShortcutRuntime } from "powerkeys"
import { type SigmaRef, SigmaType } from "preact-sigma"
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
  commandId: ShortcutCommandId
  source: ShortcutDispatchSource
  match?: ShortcutMatch
}

type ShortcutRegistryEvents = {
  [TCommandId in ShortcutCommandId]: ShortcutDispatchDetail
}

type ShortcutRegistryShape = {
  selectedProfileId: KeymapProfileId
  overrides: Partial<Record<ShortcutCommandId, string[] | null>>
  resolvedBindings: Partial<Record<ShortcutCommandId, string[]>>
  loadError: string | null
  writeError: string | null
  isHydrated: boolean
  activeScopes: string[]
  activeTabKind: WorkbenchTabKind
  hasClosableActiveTab: boolean
  selectedNavId: NavigationItemId
  overlayIsOpen: boolean
  overlayKind: string | null
  runtime: SigmaRef<ShortcutRuntime> | null
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

function getRuntimeContextSnapshot(
  state: Pick<
    ShortcutRegistryShape,
    "activeTabKind" | "hasClosableActiveTab" | "selectedNavId" | "overlayIsOpen" | "overlayKind"
  >,
) {
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
export const ShortcutRegistryModel = new SigmaType<ShortcutRegistryShape, ShortcutRegistryEvents>(
  "ShortcutRegistry",
)
  .defaultState({
    selectedProfileId: "goddard",
    overrides: {},
    resolvedBindings: getDefaultResolvedBindings,
    loadError: null,
    writeError: null,
    isHydrated: false,
    activeScopes: [],
    activeTabKind: "main",
    hasClosableActiveTab: false,
    selectedNavId: "inbox",
    overlayIsOpen: false,
    overlayKind: null,
    runtime: null,
    bindingIdsByCommand: {},
  })
  .actions({
    /** Emits one shortcut command so all listeners observe the same typed dispatch path. */
    dispatch(commandId: ShortcutCommandId, detail: Omit<ShortcutDispatchDetail, "commandId">) {
      this.emit(commandId, { commandId, ...detail })
    },

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
    },

    /** Rebuilds the runtime bindings from the current resolved keymap snapshot. */
    rebindRuntime() {
      const runtime = this.runtime

      if (!runtime) {
        return
      }

      for (const bindingIds of Object.values(this.bindingIdsByCommand)) {
        for (const bindingId of bindingIds ?? []) {
          runtime.unbind(bindingId)
        }
      }

      const nextBindingIds: Partial<Record<ShortcutCommandId, string[]>> = {}

      for (const commandId of Object.values(ShortcutCommands)) {
        const expressions = this.resolvedBindings[commandId]

        if (!expressions) {
          continue
        }

        const commandBindingIds = expressions.map(
          (expression) =>
            runtime.bind(
              createBindingInput(commandId, expression, (match) => {
                this.emit(commandId, {
                  commandId,
                  source: "keyboard",
                  match,
                })
              }),
            ).id,
        )

        if (commandBindingIds.length > 0) {
          nextBindingIds[commandId] = commandBindingIds
        }
      }

      this.bindingIdsByCommand = nextBindingIds
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

      this.runtime?.batchContext(getRuntimeContextSnapshot(this))
    },

    /** Syncs overlay-derived context keys even before any initial bindings depend on them. */
    syncOverlayContext(context: { isOpen: boolean; kind: string | null }) {
      this.overlayIsOpen = context.isOpen
      this.overlayKind = context.kind

      this.runtime?.batchContext(getRuntimeContextSnapshot(this))
    },
  })
  .setup(function () {
    const runtime = createShortcuts({
      target: document,
      editablePolicy: "ignore-editable",
      getActiveScopes: () => this.activeScopes,
      onError: (error, info) => {
        console.error("Shortcut runtime error.", error, info)
      },
    })

    this.act(function () {
      this.runtime = runtime as SigmaRef<ShortcutRuntime>
      runtime.batchContext(getRuntimeContextSnapshot(this))
      this.rebindRuntime()
    })

    return [
      () => {
        this.act(function () {
          this.runtime = null
          this.bindingIdsByCommand = {}
        })
        runtime.dispose()
      },
    ]
  })

/** Runtime instance type for the shared shortcut registry model. */
export interface ShortcutRegistry extends InstanceType<typeof ShortcutRegistryModel> {}

/** Module-scoped shortcut registry singleton shared across the app shell. */
export const shortcutRegistry = new ShortcutRegistryModel()
