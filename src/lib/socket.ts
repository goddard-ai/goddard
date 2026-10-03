import type { WebSocketLike } from '@waku/client';
import '@valor/nativescript-websockets';

declare const WebSocket: {
  new (
    url: string,
    protocols: string | string[] | null,
    options: { headers: Record<string, string> },
  ): WebSocketLike;
};

/** The daemon reads the `X-Waku-Client: native` handshake header to admit
 * native clients; @valor/nativescript-websockets accepts custom headers via
 * the same third-constructor-arg shape React Native uses. */
export function createNativeDaemonSocket(url: string): WebSocketLike {
  return new WebSocket(url, null, {
    headers: { 'X-Waku-Client': 'native' },
  });
}

/** Passed to WakuClient on native. */
export const daemonSocketFactory: ((url: string) => WebSocketLike) | undefined =
  createNativeDaemonSocket;
