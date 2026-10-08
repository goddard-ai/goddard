import { useEffect, useMemo, useRef } from 'octane';
import { WebView } from '@octane-xplat/ui';

import {
  buildTerminalHtml,
  parseTerminalMessage,
  type TerminalTheme,
  type TerminalViewHandle,
} from '@/lib/terminal-page';

/** Same-origin srcdoc + `allow-same-origin` lets both sides touch each
 * other's window: the page calls the `__goddardTerminals` registry and the
 * leaf calls `__goddardWrite` directly — no postMessage serialization. */
declare global {
  interface Window {
    __goddardTerminals?: Record<string, (message: string) => void>;
  }
}

export function TerminalView({
  bridgeId,
  theme,
  fontSize = 13,
  fontFamily = 'Menlo, monospace',
  onInput,
  onResize,
  onReady,
  bind,
  style,
  className,
}: {
  bridgeId: string;
  theme: TerminalTheme;
  fontSize?: number;
  fontFamily?: string;
  onInput?: (data: string) => void;
  onResize?: (cols: number, rows: number) => void;
  onReady?: () => void;
  bind?: (handle: TerminalViewHandle) => void;
  style?: unknown;
  className?: string;
}) {
  const propsRef = useRef({ onInput, onResize, onReady });
  propsRef.current = { onInput, onResize, onReady };
  const frameRef = useRef<HTMLIFrameElement | null>(null);

  const html = useMemo(
    () => buildTerminalHtml({ channel: 'parent', bridgeId, theme, fontFamily, fontSize }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [bridgeId],
  );

  useEffect(() => {
    const registry = (window.__goddardTerminals ??= {});
    registry[bridgeId] = (text) => {
      const message = parseTerminalMessage(text);
      if (!message) {return;}
      const handlers = propsRef.current;
      if (message.t === 'i') {handlers.onInput?.(message.d);}
      else if (message.t === 'r') {handlers.onResize?.(message.c, message.r);}
      else {handlers.onReady?.();}
    };

    return () => {
      delete registry[bridgeId];
    };
  }, [bridgeId]);

  const page = () =>
    frameRef.current?.contentWindow as
      | {
          __goddardWrite?: (data: string) => void;
          __goddardTheme?: (theme: TerminalTheme) => void;
          __goddardFocus?: () => void;
        }
      | null
      | undefined;

  useEffect(() => {
    page()?.__goddardTheme?.(theme);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [theme]);

  return (
    <WebView
      accessibilityLabel="Terminal"
      bind={(handle) => {
        frameRef.current = handle.native;
        bind?.({
          write: (data) => page()?.__goddardWrite?.(data),
          finish: (exitCode) =>
            page()?.__goddardWrite?.(`\r\n\x1b[2m— process exited (${exitCode}) —\x1b[0m\r\n`),
          setTheme: (next) => page()?.__goddardTheme?.(next),
          focus: () => page()?.__goddardFocus?.(),
        });
      }}
      html={html}
      className={className}
      style={style}
    />
  );
}
