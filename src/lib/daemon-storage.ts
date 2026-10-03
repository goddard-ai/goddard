import { SecureStorage } from '@nativescript/secure-storage';
import { storage } from '@octane-xplat/platform';

import { parseDaemonProfiles, type DaemonProfile } from './daemon-profile';

const PROFILES_KEY = 'waku.mobile.daemons.v1';
const ACTIVE_KEY = 'waku.mobile.active-daemon.v1';
const TOKEN_PREFIX = 'waku.mobile.daemon-token.v1.';

const secureStorage = new SecureStorage();

export async function readDaemonProfiles(): Promise<DaemonProfile[]> {
  const serialized = storage.getString(PROFILES_KEY);
  if (!serialized) {return [];}
  try {
    return parseDaemonProfiles(JSON.parse(serialized));
  } catch {
    return [];
  }
}

export async function writeDaemonProfiles(profiles: DaemonProfile[]): Promise<void> {
  storage.setString(PROFILES_KEY, JSON.stringify(profiles));
}

export async function readActiveDaemonId(): Promise<string | null> {
  return storage.getString(ACTIVE_KEY) ?? null;
}

export async function writeActiveDaemonId(id: string | null): Promise<void> {
  if (id) {storage.setString(ACTIVE_KEY, id);}
  else {storage.remove(ACTIVE_KEY);}
}

export async function readDaemonToken(id: string): Promise<string | null> {
  const value = await secureStorage.get({ key: tokenKey(id) });
  return typeof value === 'string' ? value : null;
}

export async function writeDaemonToken(id: string, token: string): Promise<void> {
  await secureStorage.set({ key: tokenKey(id), value: token });
}

export async function deleteDaemonToken(id: string): Promise<void> {
  await secureStorage.remove({ key: tokenKey(id) });
}

function tokenKey(id: string): string {
  return `${TOKEN_PREFIX}${id}`;
}
