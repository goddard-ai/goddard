import { expect, test } from "bun:test"

import { AppCommand, createAppCommandDetail } from "./app-command.ts"

test("createAppCommandDetail defaults session.new to a null preferred project path", () => {
  expect(
    createAppCommandDetail(AppCommand.newSession, {
      source: "programmatic",
    }),
  ).toEqual({
    source: "programmatic",
    preferredProjectPath: null,
  })
})
