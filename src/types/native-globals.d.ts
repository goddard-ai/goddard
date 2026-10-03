/** Ambient shims for the DOM-shaped APIs @waku/client names but only
 * accesses through injected factories on native (WebSocketLike's event
 * types, crypto.randomUUID injection, Storage-shaped caches). Runtime
 * values come from @valor/nativescript-websockets and app factories. */
interface MessageEvent {
  data: unknown;
}

interface CloseEvent {
  code?: number;
  reason?: string;
  wasClean?: boolean;
}

declare const WebSocket: {
  new (url: string): {
    readyState: number;
    send(data: string): void;
    close(code?: number, reason?: string): void;
    addEventListener(type: string, listener: (event: never) => void): void;
  };
};

declare const crypto: { randomUUID(): string };
interface Storage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

declare const window: any;
