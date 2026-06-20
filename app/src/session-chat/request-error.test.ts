import { IpcClientError } from "@goddard-ai/ipc"
import { SessionErrorCodes } from "@goddard-ai/sdk"
import { expect, test } from "vitest"

import { formatSessionRequestError } from "./request-error.ts"

test("formats known session IPC errors by exported code", () => {
  expect(
    formatSessionRequestError(
      new IpcClientError({
        code: SessionErrorCodes.NotActive,
        details: { sessionId: "ses_1" },
      }),
    ),
  ).toBe("This session is no longer active. Refresh the session and try again.")
})

test("falls back to daemon error messages for unknown session request errors", () => {
  expect(formatSessionRequestError(new Error("Daemon rejected the request."))).toBe(
    "Daemon rejected the request.",
  )
})

test("formats non-error rejections with a generic message", () => {
  expect(formatSessionRequestError(null)).toBe("The daemon rejected the request.")
})
