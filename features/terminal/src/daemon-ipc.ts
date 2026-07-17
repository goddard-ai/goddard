import { $type, defineIpcRoutes, http, ndjson } from "@goddard-ai/ipc"
import {
  TerminalCloseRequest,
  TerminalConnectRequest,
  TerminalCreateRequest,
  TerminalDisconnectRequest,
  TerminalEventStreamFilter,
  TerminalInputRequest,
  TerminalResizeRequest,
  TerminalRestartRequest,
  type TerminalConnectResponse,
  type TerminalCreateResponse,
  type TerminalDaemonEvent,
} from "@goddard-ai/schema/daemon/terminals"

export const terminalIpcRoutes = defineIpcRoutes({
  terminal: http.resource("terminal", {
    /** Opens one daemon terminal connection whose event stream owns connection-local terminals. */
    connect: http.post("connect", {
      body: TerminalConnectRequest,
      response: $type<TerminalConnectResponse>(),
    }),
    /** Creates one terminal instance on an existing daemon terminal connection. */
    create: http.post("create", {
      body: TerminalCreateRequest,
      response: $type<TerminalCreateResponse>(),
    }),
    /** Writes raw input to one connection-local terminal instance. */
    write: http.post("write", {
      body: TerminalInputRequest,
      response: $type<{ success: true }>(),
    }),
    /** Resizes one connection-local terminal instance. */
    resize: http.post("resize", {
      body: TerminalResizeRequest,
      response: $type<{ success: true }>(),
    }),
    /** Restarts one connection-local terminal instance. */
    restart: http.post("restart", {
      body: TerminalRestartRequest,
      response: $type<TerminalCreateResponse>(),
    }),
    /** Closes one connection-local terminal instance. */
    close: http.post("close", {
      body: TerminalCloseRequest,
      response: $type<{ success: true }>(),
    }),
    /** Disposes every terminal instance owned by one terminal connection. */
    disconnect: http.post("disconnect", {
      body: TerminalDisconnectRequest,
      response: $type<{ success: true }>(),
    }),
    /** Emits daemon terminal lifecycle events for one terminal connection. */
    event: http.get("events", {
      query: TerminalEventStreamFilter,
      response: ndjson.$type<TerminalDaemonEvent>(),
    }),
  }),
})
