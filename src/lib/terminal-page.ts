import xtermCss from '@xterm/xterm/css/xterm.css?raw';
import xtermJs from '@xterm/xterm/lib/xterm.js?raw';
import fitAddonJs from '@xterm/addon-fit/lib/addon-fit.js?raw';

/** Inline-document builder for the WebView terminal leaf. xterm.js runs
 * inside the embedded document (WKWebView / android.webkit / iframe), so the
 * whole runtime ships as one `html` string — no plugin and no extra requests.
 * Host leaves pick a `channel` for page→host messages: `webkit` uses
 * `window.webkit.messageHandlers`, `console` routes tagged `console.log`
 * lines through a WebChromeClient, and `parent` calls a registry the host
 * window owns (`window.__goddardTerminals`). */

export type TerminalChannel = 'webkit' | 'console' | 'parent';

export interface TerminalTheme {
  background: string;
  foreground: string;
  cursor: string;
  selectionBackground: string;
}

/** Imperative handle the leaf hands back through `bind`. */
export interface TerminalViewHandle {
  write(data: string): void;
  finish(exitCode: number): void;
  setTheme(theme: TerminalTheme): void;
  focus(): void;
}

/** Page→host protocol: `i` keystroke/paste data, `r` fit-driven size,
 * `ready` once xterm is open and accepting writes. */
export type TerminalMessage =
  | { t: 'i'; d: string }
  | { t: 'r'; c: number; r: number }
  | { t: 'ready' };

export const TERMINAL_CONSOLE_PREFIX = 'goddard-term:';

export function parseTerminalMessage(text: string): TerminalMessage | null {
  try {
    const message = JSON.parse(text) as TerminalMessage;
    if (message.t === 'i' && typeof message.d === 'string') {return message;}
    if (message.t === 'r' && Number.isFinite(message.c) && Number.isFinite(message.r)) {
      return message;
    }

    if (message.t === 'ready') {return message;}
  } catch {
    // Non-JSON console noise on Android shares the channel — ignore it.
  }

  return null;
}

/** `</script` inside a string/regex still ends an inline script early. The
 * `\/` escape is semantically identical JS, so the bundle text stays valid. */
function safeScript(source: string): string {
  return source.replace(/<\/script/gi, '<\\/script');
}

function pageScript(
  channel: TerminalChannel,
  bridgeId: string,
  terminalOptions: unknown,
): string {
  return `(function () {
  var CHANNEL = ${JSON.stringify(channel)};
  var BRIDGE = ${JSON.stringify(bridgeId)};
  var OPTIONS = ${JSON.stringify(terminalOptions)};
  var PREFIX = ${JSON.stringify(TERMINAL_CONSOLE_PREFIX)};
  var outbox = [];

  function rawSend(text) {
    try {
      if (CHANNEL === 'console') {
        console.log(PREFIX + text);
        return true;
      }
      if (CHANNEL === 'webkit') {
        var handlers = window.webkit && window.webkit.messageHandlers;
        if (!handlers || !handlers[BRIDGE]) { return false; }
        handlers[BRIDGE].postMessage(text);
        return true;
      }
      var bridges = window.parent && window.parent.__goddardTerminals;
      if (!bridges || !bridges[BRIDGE]) { return false; }
      bridges[BRIDGE](text);
      return true;
    } catch (error) {
      return false;
    }
  }

  function send(message) {
    var text = JSON.stringify(message);
    if (!rawSend(text)) {
      outbox.push(text);
      if (outbox.length > 64) { outbox.shift(); }
    }
  }

  setInterval(function () {
    while (outbox.length && rawSend(outbox[0])) { outbox.shift(); }
  }, 250);

  var pendingWrites = [];
  var term = null;

  window.__goddardWrite = function (data) {
    if (term) { term.write(data); }
    else { pendingWrites.push(data); }
  };
  window.__goddardTheme = function (theme) {
    if (term) { term.options.theme = theme; }
  };
  window.__goddardFocus = function () {
    if (term) { term.focus(); }
  };

  term = new Terminal(OPTIONS);
  var fit = new FitAddon.FitAddon();
  term.loadAddon(fit);
  term.open(document.getElementById('terminal'));
  try { fit.fit(); } catch (error) {}
  for (var i = 0; i < pendingWrites.length; i++) { term.write(pendingWrites[i]); }
  pendingWrites = null;

  term.onData(function (data) { send({ t: 'i', d: data }); });
  term.onBinary(function (data) { send({ t: 'i', d: data }); });
  term.onResize(function (size) { send({ t: 'r', c: size.cols, r: size.rows }); });

  var resizeTimer = 0;
  window.addEventListener('resize', function () {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(function () {
      try { fit.fit(); } catch (error) {}
    }, 60);
  });
  send({ t: 'ready' });
})();`;
}

export function buildTerminalHtml(options: {
  channel: TerminalChannel;
  bridgeId: string;
  theme: TerminalTheme;
  fontFamily: string;
  fontSize: number;
}): string {
  const terminalOptions = {
    allowProposedApi: true,
    cursorBlink: true,
    fontFamily: options.fontFamily,
    fontSize: options.fontSize,
    scrollback: 5_000,
    theme: options.theme,
  };

  const script = safeScript(
    pageScript(options.channel, options.bridgeId, terminalOptions),
  );

  return `<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<style>
html, body { height: 100%; margin: 0; overflow: hidden; overscroll-behavior: none; }
body { background: ${options.theme.background}; }
#terminal { height: 100%; width: 100%; }
#terminal .xterm { height: 100%; padding: 6px; }
${xtermCss}
</style>
</head>
<body>
<div id="terminal"></div>
<script>${safeScript(xtermJs)}</script>
<script>${safeScript(fitAddonJs)}</script>
<script>${script}</script>
</body>
</html>`;
}
