/** @jsxImportSource @nativescript-community/octane */
import { isIOS } from '@nativescript/core';
import { useCallback, useEffect, useMemo, useRef } from 'octane';
import { WebView } from '@octane-xplat/ui';

import {
  buildTerminalHtml,
  parseTerminalMessage,
  TERMINAL_CONSOLE_PREFIX,
  type TerminalTheme,
  type TerminalViewHandle,
} from '@/lib/terminal-page';

interface NativeBridge {
  /** The NS WebView view — `.ios` is WKWebView, `.android` is
   * android.webkit.WebView. */
  view: { ios?: any; android?: any } | null;
  /** The WKScriptMessageHandler instance, retained so iOS keeps delivering. */
  scriptHandler: unknown;
  chromeClient: unknown;
}

function teardownNativeView(view: NonNullable<NativeBridge['view']>, bridgeId: string) {
  if (isIOS) {
    const content = view.ios?.configuration?.userContentController;
    content?.removeScriptMessageHandlerForName(bridgeId);
    view.ios?.stopLoading?.();
  } else {
    view.android?.setWebChromeClient?.(null);
    view.android?.stopLoading?.();
  }
}

/** Native leaf for the terminal: xterm.js inside the framework's WebView.
 * Output reaches the page through evaluateJavaScript; keystrokes come back
 * over `window.webkit.messageHandlers` on iOS and tagged `console.log` lines
 * through a WebChromeClient on Android. ObjC/Java globals are reached via
 * `globalThis` so this file still typechecks under the web tsconfig. */
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

  const bridgeRef = useRef<NativeBridge>({ view: null, scriptHandler: null, chromeClient: null });

  // html is fixed at mount: rebuilding on theme/font changes would reload the
  // document and drop the scrollback. Live updates go through setTheme.
  const html = useMemo(
    () =>
      buildTerminalHtml({
        channel: isIOS ? 'webkit' : 'console',
        bridgeId,
        theme,
        fontFamily,
        fontSize,
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [bridgeId],
  );

  const runJs = useCallback((source: string) => {
    const view = bridgeRef.current.view;
    if (!view) {return;}
    try {
      if (isIOS) {
        view.ios?.evaluateJavaScriptCompletionHandler(source, null);
      } else {
        view.android?.evaluateJavascript(source, null);
      }
    } catch {
      // The webview tears itself down during sheet dismissal.
    }
  }, []);

  const deliver = useCallback((text: string) => {
    const message = parseTerminalMessage(text);
    if (!message) {return;}
    const handlers = propsRef.current;
    if (message.t === 'i') {handlers.onInput?.(message.d);}
    else if (message.t === 'r') {handlers.onResize?.(message.c, message.r);}
    else {handlers.onReady?.();}
  }, []);

  const attach = useCallback(
    (view: NativeBridge['view']) => {
      // Refs can re-fire with the same view — handler registration isn't
      // idempotent, so only bridge once.
      if (!view || bridgeRef.current.view === view) {return;}
      bridgeRef.current.view = view;
      const globals = globalThis as Record<string, any>;

      if (isIOS) {
        const Handler = globals.NSObject?.extend(
          {
            userContentControllerDidReceiveScriptMessage(_controller: unknown, message: { body: unknown }) {
              deliver(String(message?.body ?? ''));
            },
          },
          {
            name: `GoddardTerm${bridgeId.replace(/[^a-zA-Z0-9]/g, '')}`,
            protocols: [globals.WKScriptMessageHandler],
          },
        );

        const handler = Handler ? new Handler() : null;
        if (handler) {
          view.ios?.configuration?.userContentController?.addScriptMessageHandlerName(
            handler,
            bridgeId,
          );
        }

        bridgeRef.current.scriptHandler = handler;
      } else {
        const settings = view.android?.getSettings?.();
        settings?.setBuiltInZoomControls?.(false);
        settings?.setSupportZoom?.(false);
        settings?.setDisplayZoomControls?.(false);
        const ChromeClient = globals.android?.webkit?.WebChromeClient?.extend({
          onConsoleMessage(message: { message(): string }) {
            const text = String(message?.message?.() ?? '');
            if (!text.startsWith(TERMINAL_CONSOLE_PREFIX)) {return false;}
            deliver(text.slice(TERMINAL_CONSOLE_PREFIX.length));
            return true;
          },
        });

        const client = ChromeClient ? new ChromeClient() : null;
        if (client) {view.android?.setWebChromeClient?.(client);}
        bridgeRef.current.chromeClient = client;
      }
    },
    [bridgeId, deliver],
  );

  const detach = useCallback(() => {
    const bridge = bridgeRef.current;
    const view = bridge.view;
    bridge.view = null;
    if (!view) {return;}
    try {
      teardownNativeView(view, bridgeId);
    } catch {
      // Already detached.
    }

    bridge.scriptHandler = null;
    bridge.chromeClient = null;
  }, [bridgeId]);

  useEffect(() => detach, [detach]);

  useEffect(() => {
    runJs(`window.__goddardTheme && window.__goddardTheme(${JSON.stringify(theme)})`);
  }, [runJs, theme]);

  return (
    <WebView
      accessibilityLabel="Terminal"
      ref={(handle) => {
        if (!handle) {return;}
        attach(handle.native);
        bind?.({
          write: (data) => runJs(`window.__goddardWrite(${JSON.stringify(data)})`),
          finish: (exitCode) =>
            runJs(
              `window.__goddardWrite(${JSON.stringify(`\r\n\x1b[2m— process exited (${exitCode}) —\x1b[0m\r\n`)})`,
            ),
          setTheme: (next) =>
            runJs(`window.__goddardTheme && window.__goddardTheme(${JSON.stringify(next)})`),
          focus: () => runJs('window.__goddardFocus && window.__goddardFocus()'),
        });
      }}
      html={html}
      className={className}
      style={style}
    />
  );
}
