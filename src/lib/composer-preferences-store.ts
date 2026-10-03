import { storage } from '@octane-xplat/platform';
import type { RuntimeMode } from '@waku/client';
import {
  readComposerPreferences,
  writeComposerPreferences,
  type ComposerPreferences,
} from '@waku/client/composer-preferences';

/** Mobile-only additions to the shared composer preferences: the New Task
 * page also restores access mode, workspace choice, and project. */
export interface NewTaskExtras {
  runtimeMode: RuntimeMode;
  isolated: boolean;
  projectId: string | null;
}

const EXTRAS_KEY = 'waku.mobile.new-task.v1';
const DEFAULT_EXTRAS: NewTaskExtras = {
  runtimeMode: 'autoAcceptEdits',
  isolated: false,
  projectId: null,
};

/** ApplicationSettings/localStorage are already synchronous, so the shared
 * preference modules get a direct Storage view — the reference app's async
 * hydration dance collapses to a pass-through. */
const syncStorage: Pick<Storage, 'getItem' | 'setItem'> = {
  getItem: (key) => storage.getString(key),
  setItem: (key, value) => {
    storage.setString(key, value);
  },
};

/** Kept for call-site parity; persistence needs no hydration on xplat. */
export function hydratePersistentStorage(): Promise<void> {
  return Promise.resolve();
}

export function persistentStorageSync(): Pick<Storage, 'getItem' | 'setItem'> | null {
  return syncStorage;
}

export async function loadComposerPreferences(
  daemonAddress: string,
): Promise<ComposerPreferences> {
  return readComposerPreferences(syncStorage, daemonAddress);
}

export async function saveComposerPreferences(
  daemonAddress: string,
  preferences: ComposerPreferences,
): Promise<void> {
  writeComposerPreferences(syncStorage, daemonAddress, preferences);
}

export async function loadNewTaskExtras(daemonAddress: string): Promise<NewTaskExtras> {
  try {
    const entries = JSON.parse(syncStorage.getItem(EXTRAS_KEY) ?? '{}') as Record<string, unknown>;
    const value = entries[daemonAddress];
    if (typeof value !== 'object' || value === null) {return { ...DEFAULT_EXTRAS };}
    const extras = value as Partial<NewTaskExtras>;
    return {
      runtimeMode: isRuntimeMode(extras.runtimeMode) ? extras.runtimeMode : 'autoAcceptEdits',
      isolated: extras.isolated === true,
      projectId: typeof extras.projectId === 'string' ? extras.projectId : null,
    };
  } catch {
    return { ...DEFAULT_EXTRAS };
  }
}

export async function saveNewTaskExtras(
  daemonAddress: string,
  extras: NewTaskExtras,
): Promise<void> {
  let entries: Record<string, unknown> = {};
  try {
    const parsed = JSON.parse(syncStorage.getItem(EXTRAS_KEY) ?? '{}');
    if (typeof parsed === 'object' && parsed !== null && !Array.isArray(parsed)) {
      entries = parsed as Record<string, unknown>;
    }
  } catch {
    // Malformed disposable state; replace it.
  }

  entries[daemonAddress] = extras;
  syncStorage.setItem(EXTRAS_KEY, JSON.stringify(entries));
}

function isRuntimeMode(value: unknown): value is RuntimeMode {
  return value === 'ask' || value === 'autoAcceptEdits' ||
    value === 'auto' || value === 'fullAccess';
}
