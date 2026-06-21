# Browser Access

Browser access lets a web page or desktop webview call the local daemon over loopback HTTP without treating CORS as authorization. It is disabled by default and must be enabled explicitly in daemon configuration.

## Configuration

Enable browser access in the daemon config and list exact origins:

```json
{
  "daemon": {
    "browserAccess": {
      "enabled": true,
      "allowedOrigins": ["https://app.goddardai.org"],
      "desktopWebviewOrigins": ["http://localhost:5173"]
    }
  }
}
```

- `allowedOrigins` are hosted web origins that may start and complete browser pairing.
- `desktopWebviewOrigins` are trusted app webview origins that may use host-bootstrapped desktop tokens.
- Origins must be exact URL origins. Do not use `*`, `null`, paths, queries, or hashes.
- If browser access is omitted or not enabled, browser-origin requests are rejected.

The daemon still defaults to `http://127.0.0.1:49827/` unless the normal daemon port settings override it. Existing local Node and desktop host clients that do not send an `Origin` header keep using trusted local IPC behavior.

## Hosted Browser Pairing

A hosted browser origin does not become trusted just because it is allowlisted. The browser must complete an explicit local pairing flow:

1. The hosted browser calls `daemon.browserAccess.pairing.start` from an allowed origin.
2. The daemon returns a short numeric code and pending pairing id.
3. A trusted local client, such as the desktop host, CLI, or future tray UI, confirms that pairing id and code through local IPC.
4. The hosted browser calls `daemon.browserAccess.pairing.complete` from the same origin.
5. The daemon returns a bearer token bound to that origin.

The browser sends the token as:

```http
Authorization: Bearer <token>
```

The daemon stores only a one-way token digest. Replaying the token from a different origin is rejected, even when that other origin is also allowlisted. Revoking a paired browser client immediately removes access without restarting the daemon.

## Desktop Webview Bootstrap

The desktop app keeps trust anchored in the Bun host process. The Bun host can ask the daemon for a short-lived webview token for the current desktop webview origin. The webview then calls the daemon directly over loopback with that token.

Desktop webview tokens are in-memory and scoped to the current app/webview session. If the daemon rejects the token before dispatching a request, the renderer may ask the Bun host for a fresh token and retry that request once. The webview token is still origin-bound; possession of the token alone is not enough.

The desktop host remains responsible for native capabilities such as dialogs, window controls, app-local files, and runtime startup. Normal daemon request and stream IPC should use the direct browser daemon client path.

## Browser Permission Behavior

Browsers may apply Private Network Access checks when a public web page calls `http://127.0.0.1:<port>/`. The daemon answers allowed PNA preflights with `Access-Control-Allow-Private-Network: true`, but browser prompt and enforcement behavior is browser-specific.

Manual smoke check:

1. Start the daemon with browser access enabled for the page origin.
2. Open the page from that exact origin.
3. Pair the browser through the local confirmation flow.
4. Call `http://127.0.0.1:49827/daemon/health` or another daemon route through the browser client.
5. Confirm the request includes the bearer token and the browser reports no CORS or PNA denial.

## Troubleshooting

- `403` on preflight or no CORS headers: the request origin is missing, malformed, `null`, `*`, or not listed in daemon browser access config.
- Browser PNA prompt or failure: confirm the origin is allowlisted and the preflight response includes `Access-Control-Allow-Private-Network: true`; browser UI may still require user permission.
- Daemon unavailable or connection error: confirm the daemon is running and the browser is using the configured loopback URL and port.
- Pairing fails before token issuance: confirm the local trusted surface has confirmed the current pairing id and short code before the browser completes pairing.
- Token works from one origin but not another: hosted-browser and desktop-webview tokens are origin-bound by design.
- Revoked browser client gets `403`: pair the browser again; revocation is immediate and does not require daemon restart.
