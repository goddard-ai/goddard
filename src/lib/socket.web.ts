import type { WebSocketLike } from '@waku/client';

/** Browsers cannot set handshake headers — leave the factory undefined so
 * WakuClient uses its built-in `new WebSocket(url)` default. */
export function createNativeDaemonSocket(_url: string): WebSocketLike {
  throw new Error('createNativeDaemonSocket is native-only');
}

export const daemonSocketFactory: ((url: string) => WebSocketLike) | undefined =
  undefined;
